# Hive 🐝

Hive is a Lua microservices framework written in Rust. It provides an easy and fun way of writing JSON RESTful APIs while maintaining great performance.

[Documentation]() (coming soon)

*Note: Hive is currently under heavy development, and many functionalities are yet to be implemented. Nevertheless, feel free to try it out, and any feedback would be appreciated!*

## Features

- Fully multi-threaded & asynchronous
- Writing, deploying and iterating Lua microservices without breaking a sweat
- Compliant to HTTP standards and JSON RESTful API conventions
- Sandbox environment, with limited yet powerful Sandbox API

## Getting Started

Write a hello world service and save it to `hello.lua`:
```lua
hive.register("/:name", function(req)
  return { greeting = "Hello, " .. req.params.name .. "!" }
end)
```

Run Hive:
```sh
$ cargo run
```

In another shell, upload the source code and run the service:
```sh
$ curl localhost:3000/services/hello -X PUT -F source=@hello.lua | jq
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

Hive currently uses Lua 5.4 as its runtime. Lower versions and LuaJIT support is under consideration for now.

## License

Hive is licensed under the MIT License.
