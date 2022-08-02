# Abel 🐝

[![GitHub release](https://img.shields.io/github/v/release/hack3ric/abel)](https://github.com/hack3ric/abel/releases)
[![License](https://img.shields.io/github/license/hack3ric/abel)](https://github.com/hack3ric/abel/blob/master/LICENSE)
[![Rust CI](https://github.com/hack3ric/abel/actions/workflows/rust.yml/badge.svg)](https://github.com/hack3ric/abel/actions/workflows/rust.yml)

Abel is a lightweight microservices framework for Lua. It focuses on simple, fun experience of writing modular web services.

See [Documentation](https://hack3ric.github.io/abel-doc) and [Roadmap](https://hack3ric.github.io/abel-doc/roadmap) for more information.

*Abel is currently under heavy development, and many functionalities are yet to be implemented. Nevertheless, feel free to try it out, and any feedback would be appreciated!*

## Why Abel?

You want Abel when:

- you are tired of compiling, packaging, logging in, and deploying even the simpliest service you write on your server;
- you want to Cloudflare Workers, but self-hosted;
- you want an out-of-the-box experience of writing and deploying web services.

You don't want Abel when:

- you build complex web services; (maybe one day it can do so too?)
- performance is your main goal; (still decent performance though)
- you want to access the entire filesystem, spawn child processes or use FFI libraries.

## Quick Start

Write a hello world service and save it to `hello.lua`:
```lua
abel.listen("/:name", function(req)
  return { greeting = "Hello, " .. req.params.name .. "!" }
end)
```

Run Abel:
```console
$ cargo run -- dev hello.lua
 INFO  abel > Starting abel-server v0.1.0 (dev mode)
 INFO  abel::server > Loaded service (0b684ecb-029e-40ee-8757-9f34fdc2e662)
 INFO  abel::server > Abel is listening to 127.0.0.1:3000
```

In another shell, run the service:
```console
$ curl localhost:3000/hello/world | jq
{
  "greeting": "Hello, world!"
}
```

Deploy the service in one request:

```console
$ curl https://abel.example.com/services/hello \
  -H "Authorization: Abel <your-auth-token>" \
  -X PUT \
  -F single=@hello.lua | jq
{
  "new_service": {
    "name": "hello",
    // ...
  }
}

$ curl https://abel.example.com/hello/server | jq
{
  "greeting": "Hello, server!"
}
```

## Lua Version Compatibility

Abel currently uses Lua 5.4 as its runtime. Lower versions and LuaJIT support is under consideration for now.

## License

Abel is licensed under the MIT License.
