local crypto = require "crypto"
local http = require "http"

local UriError = HttpError {
  status = 400,
  error = "failed to parse URI",
  detail = function(msg)
    return { msg = msg }
  end
}

abel.listen("/*", function(req)
  local raw_uri = req.params["*"]
  local success, uri_or_err = pcall(http.Uri, raw_uri)
  if not success then
    error(UriError(uri_or_err))
  end
  local uri_hash = crypto.Sha256(raw_uri)

  local resp = http.request(uri_or_err)
  local hasher = crypto.Sha256()
  resp.body:pipe_to(hasher)
  local body_hash = hasher:finalize()

  return {
    uri_hash = uri_hash,
    body_hash = body_hash,
  }
end)
