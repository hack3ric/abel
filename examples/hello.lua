local function hello(req)
  local name = req.params.name or "world"
  return { greeting = "Hello, " .. name .. "!" }
end

hive.register("/", hello)
hive.register("/:name", hello)
