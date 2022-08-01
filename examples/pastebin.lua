-- A simple one-time pastebin.
--
-- This example will keep updating with upcoming new features of Abel.

local crypto = require "crypto"
local fs = require "fs"
local http = require "http"

local SIZE_THRESHOLD = 1048576

local MethodNotAllowed = HttpError {
  status = 405,
  error = "method not allowed",
  detail = function(got, allowed)
    return { got = got, allowed = allowed }
  end
}

local FileTooLarge = HttpError {
  status = 413,
  error = "file too large",
  detail = {
    max = SIZE_THRESHOLD
  }
}

local function gen_uid()
  local result = ""
  for _ = 1, 8 do
    local v = crypto.ThreadRng:gen_range(0, 0xf)
    result = result .. ("%x"):format(v)
  end
  return result
end

function abel.start()
  fs.mkdir "files"
end

-- Upload file
abel.listen("/", function(req)
  if req.method ~= "POST" then
    error(MethodNotAllowed(req.method, { "POST" }))
  end

  local size = tonumber(req.headers.content_length)
  if size and size > SIZE_THRESHOLD then
    error(FileTooLarge { got = size })
  end

  local uid = gen_uid()
  local file <close> = fs.open("files/" .. uid, "w")

  local limiter = {
    size = 0,
    transform = function(self, bytes)
      self.size = self.size + #bytes
      if self.size > SIZE_THRESHOLD then
        fs.remove("files/" .. uid)
        file:close()
        error(FileTooLarge)
      end
      return bytes
    end
  }

  req.body
    :pipe_through(limiter)
    :pipe_to(file)

  return { uid = uid }
end)

-- Download file
abel.listen("/:uid", function(req)
  if req.method ~= "GET" then
    error(MethodNotAllowed(req.method, { "GET" }))
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
      content_type = "text/plain",
      content_length = metadata.size,
    },
    body = file
  }
end)
