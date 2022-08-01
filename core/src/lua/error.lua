local lua_error = error
local handle_http_error, pcall = ...

function error(msg, level)
  if type(msg) == "table" then
    handle_http_error(msg)
  end
  lua_error(msg, (level or 1) + 1)
end

function assert(pred, msg)
  if pred then return pred end
  error(msg or "assertion failed!", 2)
end

_G.pcall = pcall
