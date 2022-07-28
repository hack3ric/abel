local function hello(req)
  local name = req.params.name or "world"
  return { greeting = "Hello, " .. name .. "!" }
end

abel.listen("/", hello)
abel.listen("/:name", hello)
