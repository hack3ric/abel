---@diagnostic disable: undefined-global
local function sleep_print(ms)
  abel.sleep(ms)
  print("Sleep " .. ms .. "ms")
end

local function hello(req)
  abel.spawn(
    abel.await_all,
    bind(sleep_print, 1000),
    bind(sleep_print, 2000)
  )

  local name = req.params.name or "world"
  return { greeting = "Hello, " .. name .. "!" }
end

abel.listen("/", hello)
abel.listen("/:name", hello)
