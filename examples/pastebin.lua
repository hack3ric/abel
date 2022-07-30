-- A simple one-time pastebin.
--
-- This example will keep updating with upcoming new features of Abel.

local crypto = require "crypto"
local fs = require "fs"
local http = require "http"

local method_not_allowed = HttpError {
  status = 405,
  error = "method not allowed",
  detail = function(got, allowed)
    return { got = got, allowed = allowed }
  end,
}

local function gen_uid()
  local result = ""
  for _ = 1, 8 do
    local v = crypto.thread_rng:gen_range(0, 0xf)
    result = result .. string.format("%x", v)
  end
  return result
end

function abel.start()
  fs.mkdir "files"
end

abel.listen("/", function(req)
  if req.method ~= "POST" then
    error(method_not_allowed(req.method, { "POST" }))
  end

  local content = req.body:to_string()
  local uid = gen_uid()

  local file <close> = assert(io.open("files/" .. uid, "w"))
  assert(file:write(content))

  return { uid = uid }
end)

abel.listen("/:uid", function(req)
  if req.method ~= "GET" then
    error(method_not_allowed(req.method, { "GET" }))
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

  local metadata = assert(fs.metadata(path))

  -- This works on POSIX systems, but not Windows
  assert(os.remove(path))

  return http.Response {
    headers = {
      ["content-type"] = "text/plain",
      ["content-length"] = tostring(metadata.size)
    },
    body = file:into_stream(),
  }
end)
