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
$ cargo run -- server
 INFO  abel::server > Authentication token: <your-auth-token>
 INFO  abel::server > Abel is listening to 127.0.0.1:3000
```

In another shell, upload the source code and run the service:
```console
$ curl localhost:3000/services/hello \
    -X PUT -F single=@hello.lua \
    -H "Authorization: Abel <your-auth-token>" \
    | jq
{
  "new_service": {
    "name": "hello"
    ...
  }
}
$ curl localhost:3000/hello/world | jq
{
  "greeting": "Hello, world!"
}
```

<!-- ## Features

- Fully multi-threaded & asynchronous
- Writing, deploying and iterating Lua microservices without breaking a sweat
- Compliant to HTTP standards and JSON RESTful API conventions
- Sandbox environment, with limited yet powerful Lua API

## Basic Concepts

Abel follows three major principles:

- **Asynchronous.** Reading a file, requesting an API, or other things that can be made asynchronous, will never block other instance to execute.

- **Sandboxed.** an Abel service can only access its own contained resources, and is fully blocked from the outside world.

- **Standardized.** Abel conforms with HTTP standards and RESTful JSON API conventions (unless you want to break them intentionally).

Thanks to [Rust](https://rust-lang.org), [Tokio](https://tokio.rs), [Hyper](https://hyper.rs) and [Lua](https://lua.org) (as well as its binding [mlua](https://github.com/khvzak/mlua)), these ideal designs are way easier to realize. -->

## Lua Version Compatibility

Abel currently uses Lua 5.4 as its runtime. Lower versions and LuaJIT support is under consideration for now.

## License

Abel is licensed under the MIT License.
