# Robby

Robby is an ingress web proxy for your Nomad cluster.
Just add a tag to your Nomad web service, and Robby will pick it up and proxy connections to your service.

## Example

If I host `example.com` in my Nomad cluster, I add the tag `"urlprefix-example.com/"` to the `service` section
of my nomad service config:
```
service {
    name = "example-com"
    tags = ["global", "cache", "urlprefix-example.com/"]
    ...
}
```

That's it. Robby will find that `urlprefix-`, and route any incoming web requests with header `Host: example.com` to wherever nomad hosts that service. Robby keeps up to date with Nomad, and will correctly route as your service moves around the cluster.

Wildcards also work. You can set your `urlprefix-` to, e.g. `"urlprefix-*example.com"` to route `example.com` and any subdomains to that service.

The `urlprefix-` is meant to be compatible with (fabio)[https://github.com/fabiolb/fabio] but only a subset (host matching) is implemented currently.
