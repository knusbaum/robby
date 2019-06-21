# Robby

[![Build Status](https://travis-ci.com/knusbaum/robby.svg?branch=master)](https://travis-ci.com/knusbaum/robby)

Robby is an ingress web proxy for your [Nomad](https://www.nomadproject.io/) cluster.
Just add a tag to your Nomad web service, and Robby will pick it up and proxy connections to your service.

## Example

If I host `example.com` in my Nomad cluster, I add the tag `"urlprefix-example.com/"` to the `service` section
of my Nomad service config:
```
service {
    name = "example-com"
    tags = ["global", "cache", "urlprefix-example.com/"]
    ...
}
```

That's it. Robby will find that `urlprefix-`, and route any incoming web requests with header `Host: example.com` to wherever Nomad hosts that service. Robby keeps up to date with Nomad, and will correctly route as your service moves around the cluster.

Wildcards also work. You can set your `urlprefix-` to, e.g. `"urlprefix-*example.com"` to route `example.com` and any subdomains to that service.

The `urlprefix-` is meant to be compatible with [fabio](https://github.com/fabiolb/fabio) but only a subset (host matching) is implemented currently.


## Config
Robby looks for `/etc/robby.yml` for configuration. There's a sample config called `robby.yml` in this repo.
If no config is present, robby uses the default listening ip and port of `0.0.0.0:9001`


## Performance
See [load testing with locust](locust)


## Local test

If you want to test it out locally, the easiest way is to bring up a `consul` docker container:
```
docker run -d -p8500:8500 consul:latest
```

Have something to proxy to. For example, listening on `0.0.0.0:8000`.

Then, register a service with consul, including a `urlprefix-` tag. If robby is listening on the default port (9001), and your web service is accepting connections on `127.0.0.1:8000`, then this should work:
```
curl -X PUT -H "Content-Type: application/json" -d '{ "Name": "TEST", "Tags": ["urlprefix-localhost:9001"], "Port": 8000, "Address": "127.0.0.1" }' http://127.0.0.1:8500/v1/agent/service/register
```
Adjust the `"Port"` and `"Address"` fields as necessary.

Run robby
```
cargo run
```

Finally, navigate to [`http://localhost:9001`](http://localhost:9001).
