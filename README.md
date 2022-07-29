# Abel 🐝

Abel is a lightweight microservices framework for Lua. It focuses on simple, fun experience of writing modular web services.

See [Documentation](https://hack3ric.github.io/abel-doc) for more information.

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

## Lua Version Compatibility

Abel currently uses Lua 5.4 as its runtime. Lower versions and LuaJIT support is under consideration for now.

## License

Abel is licensed under the MIT License.
