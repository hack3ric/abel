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
    result = result .. ("%x"):format(v)
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

  local uid = gen_uid()
  local file <close> = fs.open("files/" .. uid, "w")
  req.body:pipe_to(file)

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
  if not fs.exists(path) then
    error { status = 404, error = "file not found" }
  end
  local file = fs.open(path)
  local metadata = fs.metadata(path)

  -- This works on POSIX systems, but not Windows
  fs.remove(path)

  return http.Response {
    headers = {
      ["content-type"] = "text/plain",
      ["content-length"] = metadata.size,
    },
    body = file,
  }
end)
