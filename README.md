# Docker registry server and proxy

(I'm bad at creating catchy names, but this one is good enough.)

This project aims to implement a [Docker Registry HTTP API](https://docs.docker.com/registry/spec/api/) that can also act as a proxy to other registries with a dedicated path for that purpose. In other words, it's a storage for your own Docker images and a cache for others. If you're using GitLab, this will probably remind you of the Container Registry and the Dependency Proxy.

**Note: This project is not production-ready and should be used at your own risk !**

## Why not [Docker's Go implementation](https://github.com/distribution/distribution/) of the registry ?

While installing the registry was a breeze, I can't make it work as a pull-through registry and a storage registry **at the same time**. It's either one, or the other. While I could set up another registry (pretty much automated with Ansible), that would mean grabbing another TLS certificate, setting up another subdomain and configure the HTTP reverse proxy appropriately. Let alone configuring the HTTP reverse-proxy to use only one domain.

## Non-goals

Implementing the entire [Docker Registry HTTP API V2 specification](https://docs.docker.com/registry/spec/api/) is a non-goal. As long as I can push and pull images with the `docker` client, I will be fine. If I find tooling that needs [monolithic blob uploads](https://docs.docker.com/registry/spec/api/#post-initiate-blob-upload), I will maybe look into implementing that. Same goes for other URIs in the specification.

## Current limitations
In the current state of the code (2022-12-13, commit `afb86448`), there are a few limitations. Some can be compensated, others not quite.

1. No HTTP authentication. You must set up a reverse proxy for that, although chances are you already have one.
2. Docker registry connection information — e.g. tokens to access the DockerHub anonymously — aren't cleaned when the token expires. They are recreated when the proxy needs to hit the upstream registry and the token has expired.
3. Garbage collecting the proxy container registry has not been developed yet.

## Configuration sample

A file named `configuration.toml` in the server's working directory will suffice. The file should contain the following keys, otherwise it won't start:

```toml
registry_storage = "storage/registry"
temporary_registry_storage = "storage/_tmp"
proxy_storage = "storage/proxy"
```

## A few words on the container proxy
If proxying containers, you **must** give the registry the **whole** path to reach the container, especially for containers from the DockerHub. Otherwise, you may end up with issues regarding DNS not resolving addresses.

For example, if you want to reference `hello-world:latest` from the DockerHub, you must reference it with `registry-1.docker.io/library/hello-world:latest`. The whole URL will look like `<your registry>/proxy/registry-1.docker.io/library/hello-world:latest`. It's long-winded, but in the interest of keeping things simple with regular expressions, this will do. Containers from other registries are not affected since you must refer to them by the whole path anyway.

## License
Copyright 2022 Mathias B. <contact@l4p1n.ch>

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.