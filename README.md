# Abel 🐝

Abel (formerly Hive) is a Lua microservices framework written in Rust. It provides an easy and fun way of writing JSON RESTful APIs while maintaining great performance.

*Abel is currently under heavy development, and many functionalities are yet to be implemented. Nevertheless, feel free to try it out, and any feedback would be appreciated!*

## Features

- Fully multi-threaded & asynchronous
- Writing, deploying and iterating Lua microservices without breaking a sweat
- Compliant to HTTP standards and JSON RESTful API conventions
- Sandbox environment, with limited yet powerful Sandbox API

## Basic Concepts

Abel follows three major principles:

- **Asynchronous.** Reading a file, requesting an API, or other things that can be made asynchronous, will never block other instance to execute.

- **Sandboxed.** an Abel service can only access its own contained resources, and is fully blocked from the outside world.

- **Standardized.** Abel conforms with HTTP standards and RESTful JSON API conventions (unless you want to break them intentionally).

Thanks to [Rust](https://rust-lang.org), [Tokio](https://tokio.rs), [Hyper](https://hyper.rs) and [Lua](https://lua.org) (as well as its binding [mlua](https://github.com/khvzak/mlua)), these ideal designs are way easier to realize.

## Getting Started

Write a hello world service and save it to `hello.lua`:
```lua
abel.register("/:name", function(req)
  return { greeting = "Hello, " .. req.params.name .. "!" }
end)
```

Run Abel:
```console
$ cargo run
```

In another shell, upload the source code and run the service:
```console
$ curl localhost:3000/services/hello -X PUT -F single=@hello.lua | jq
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

## Lua Version Compatibility

Abel currently uses Lua 5.4 as its runtime. Lower versions and LuaJIT support is under consideration for now.

## License

Abel is licensed under the MIT License.
