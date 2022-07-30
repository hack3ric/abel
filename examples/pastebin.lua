-- A simple one-time pastebin.
--
-- This example will keep updating with upcoming new features of Abel.

local crypto = require "crypto"
local fs = require "fs"

local method_not_allowed = HttpError {
  status = 405,
  error = "method not allowed",
}

local function gen_uid()
  local template = "xxxxxxxx"
  return (string.gsub(template, "x", function(c)
    local v = crypto.thread_rng:gen_range(0, 0xf)
    return string.format('%x', v)
  end))
end

function abel.start()
  fs.mkdir "files"
end

abel.listen("/", function(req)
  if req.method ~= "POST" then
    error(method_not_allowed {
      allowed = { "POST" },
      got = req.method,
    })
  end

  local content = req.body:to_string()
  local uid = gen_uid()

  local file <close> = assert(io.open("files/" .. uid, "w"))
  assert(file:write(content))

  return { uid = uid }
end)

abel.listen("/:uid", function(req)
  if req.method ~= "GET" then
    error(method_not_allowed {
      allowed = { "GET" },
      got = req.method,
    })
  end

  local uid = req.params.uid
  if #uid ~= 8 then
    error { status = 400, error = "invalid UID" }
  end

  local path = "files/" .. uid
  local file = io.open(path)
  if not file then
    error { status = 404, error = "file not found" }
  end

  -- This works on POSIX systems, but not Windows
  assert(os.remove(path))
  return file:into_stream()
end)
