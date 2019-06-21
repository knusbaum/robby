use std::{collections::HashMap, net::SocketAddr, sync::RwLock};

use rand::{seq::SliceRandom, thread_rng};

use knusbaum_consul;

#[derive(Debug)]
pub struct AddressPort {
    pub address: String,
    pub port: u16,
}

#[derive(Debug)]
pub enum GetHostError {
    PoisonErr(String),
    StrErr(String),
}

// This trait is only here for use in tests right now.
// In the future, I would like to make this trait more generic to allow
// for other ServiceProviders to be used with the proxy.
pub trait ServiceProvider {
    fn services(&self) -> Result<HashMap<String, Vec<String>>, String>;
    fn get_nodes(&self, service: String) -> Result<Vec<knusbaum_consul::ServiceNode>, String>;
}

impl ServiceProvider for knusbaum_consul::Client {
    fn services(&self) -> Result<HashMap<String, Vec<String>>, String> {
        self.catalog.services()
    }

    fn get_nodes(&self, service: String) -> Result<Vec<knusbaum_consul::ServiceNode>, String> {
        self.catalog.get_nodes(service)
    }
}

#[derive(Debug)]
pub struct ServiceRegistry<T: ServiceProvider = knusbaum_consul::Client> {
    services: RwLock<HashMap<String, Vec<AddressPort>>>,
    client: T,
}

impl<T: ServiceProvider> ServiceRegistry<T> {
    pub fn new() -> ServiceRegistry<knusbaum_consul::Client> {
        ServiceRegistry {
            services: RwLock::new(HashMap::new()),
            client: knusbaum_consul::Client::new("http://127.0.0.1:8500"),
        }
    }

    pub fn update(&self) -> Result<(), String> {
        let new_map = self.pull_consul_routes()?;
        let mut locked = self.services.write().map_err(|e| format!("{:?}", e))?;
        *locked = new_map;
        Ok(())
    }

    pub fn lookup(&self, host: &str) -> Result<SocketAddr, GetHostError> {
        let address = self.address_for_host(host);
        if address.is_ok() {
            return address;
        }

        // This is gross. Need to replace this with true globbing.
        let parts: Vec<&str> = host.split(".").collect();
        let mut partslice = &parts[..];
        while partslice.len() > 0 {
            let tryhost = format!("*{}", partslice.join("."));
            let address = self.address_for_host(&tryhost);
            if address.is_ok() {
                return address;
            }
            partslice = &partslice[1..];
        }
        return Err(GetHostError::StrErr(format!(
            "No address found for host {}",
            host
        )));
    }

    fn address_for_host(&self, host: &str) -> Result<SocketAddr, GetHostError> {
        let services = self
            .services
            .read()
            .map_err(|e| GetHostError::PoisonErr(format!("{:?}", e)))?;

        match services.get(host) {
            Some(addresses) => {
                let mut rng = thread_rng();
                match addresses.choose(&mut rng) {
                    Some(address) => {
                        return format!("{}:{}", address.address, address.port)
                            .parse()
                            .map_err(|e| {
                                GetHostError::StrErr(format!("Failed to parse address: {:?}", e))
                            });
                    }
                    None => {
                        return Err(GetHostError::StrErr(format!(
                            "No address found for host {}",
                            host
                        )));
                    }
                }
            }
            None => {
                return Err(GetHostError::StrErr(format!(
                    "No address found for host {}",
                    host
                )));
            }
        }
    }

    fn add_address_port(
        service_map: &mut HashMap<String, Vec<AddressPort>>,
        service: String,
        address_port: AddressPort,
    ) {
        if let Some(address_ports) = service_map.get_mut(&service) {
            address_ports.push(address_port);
        } else {
            service_map.insert(service.to_string(), vec![address_port]);
        }
    }

    fn extract_prefix(tag: &str) -> String {
        tag.replace("urlprefix-", "").replace("/", "")
    }

    fn pull_consul_routes(&self) -> Result<HashMap<String, Vec<AddressPort>>, String> {
        let services: HashMap<String, Vec<String>> = self.client.services()?;
        let mut service_map: HashMap<String, Vec<AddressPort>> = HashMap::new();
        for service in services.keys() {
            let nodes = self.client.get_nodes(service.to_string())?;
            for node in nodes.into_iter() {
                for tag in node.ServiceTags.into_iter() {
                    if tag.starts_with("urlprefix-") {
                        Self::add_address_port(
                            &mut service_map,
                            Self::extract_prefix(&tag),
                            AddressPort {
                                address: node.ServiceAddress.clone(),
                                port: node.ServicePort,
                            },
                        );
                    }
                }
            }
        }
        Ok(service_map)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub struct TestConsul {
        hostname: String,
        target_port: u16,
    }

    impl ServiceProvider for TestConsul {
        fn services(&self) -> Result<HashMap<String, Vec<String>>, String> {
            let mut m = HashMap::new();
            m.insert(
                "test_service".to_string(),
                vec![format!("urlprefix-{}/", self.hostname)],
            );
            return Ok(m);
        }

        fn get_nodes(&self, service: String) -> Result<Vec<knusbaum_consul::ServiceNode>, String> {
            if service == "test_service" {
                return Ok(vec![knusbaum_consul::ServiceNode {
                    Address: "127.0.0.1".to_string(),
                    Node: "0923e94c789a".to_string(),
                    ServiceAddress: "127.0.0.1".to_string(),
                    ServiceID: "test_service".to_string(),
                    ServiceName: "test_service".to_string(),
                    ServicePort: self.target_port,
                    ServiceTags: vec![format!("urlprefix-{}/", self.hostname)],
                }]);
            }
            return Err("No such service.".to_string());
        }
    }

    pub fn test_registry(hostname: &str, target_port: u16) -> ServiceRegistry<TestConsul> {
        ServiceRegistry {
            services: RwLock::new(HashMap::new()),
            client: TestConsul {
                hostname: hostname.to_string(),
                target_port: target_port,
            },
        }
    }

    #[test]
    fn test_lookup() {
        let registry = test_registry("test-website.com", 8080);
        assert!(registry.update().is_ok());
        let result = registry.lookup("test-website.com");
        assert!(result.is_ok());

        let addr = result.unwrap();
        assert_eq!(addr, "127.0.0.1:8080".parse().unwrap());
    }

    #[test]
    fn test_pull_consul_routes() {
        let registry = test_registry("test-website.com", 8080);
        let result = registry.pull_consul_routes();
        assert!(result.is_ok());
        let result = result.unwrap();

        let addrs = result.get("test-website.com");
        assert!(addrs.is_some());
        let addrs = addrs.unwrap();

        assert!(addrs.len() == 1);
        assert!(addrs[0].address == "127.0.0.1");
        assert!(addrs[0].port == 8080);
    }

    fn check_matches(host: &str, service_prefix: &str) {
        let addrs = vec![AddressPort {
            address: "127.0.0.1".to_string(),
            port: 8080,
        }];
        let mut map = HashMap::new();
        map.insert(service_prefix.to_string(), addrs);
        let registry = ServiceRegistry {
            services: RwLock::new(map),
            client: TestConsul {
                hostname: "".to_string(),
                target_port: 0,
            },
        };

        // Test
        let target = registry.lookup(host);
        assert!(target.is_ok());
        assert_eq!(target.ok().unwrap(), "127.0.0.1:8080".parse().unwrap());
    }

    fn check_no_match(host: &str, service_prefix: &str) {
        let addrs = vec![AddressPort {
            address: "127.0.0.1".to_string(),
            port: 8080,
        }];
        let mut map = HashMap::new();
        map.insert(service_prefix.to_string(), addrs);
        let registry = ServiceRegistry {
            services: RwLock::new(map),
            client: TestConsul {
                hostname: "".to_string(),
                target_port: 0,
            },
        };

        // Test
        let target = registry.lookup(host);
        assert!(target.is_err());
    }

    #[test]
    fn test_host_matching() {
        // *.foo.com globbing"
        check_matches("foo.com", "*foo.com");
        check_matches("bar.foo.com", "*foo.com");
        check_matches("baz.bar.foo.com", "*foo.com");
        check_no_match("foo.com.biz", "*foo.com");

        check_matches("foo.com", "foo.com");
        check_no_match("bar.foo.com", "foo.com");
        check_no_match("foo.com.biz", "foo.com");
    }

    fn check_extract_prefix(prefix: &str, expect: &str) {
        assert_eq!(&<ServiceRegistry>::extract_prefix(prefix), expect);
    }

    #[test]
    fn test_extract_prefix_host_match() {
        check_extract_prefix("urlprefix-foo.com/", "foo.com");
        check_extract_prefix("urlprefix-*.foo.com/", "*.foo.com");
    }
}
