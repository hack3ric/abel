local bstr_debug_fmt = ...

function HttpError(obj)
  local status, error, detail = obj.status, obj.error, obj.detail

  local result = {
    status = status,
    error = error,
    detail = detail,
  }
  local result_mt = {
    __call = function(self, detail)
      return {
        status = status,
        error = error,
        detail = detail,
      }
    end
  }

  return setmetatable(result, result_mt)
end

local lua_getmetatable = getmetatable

function getmetatable(t)
  local type_t = type(t)
  assert(
    type_t == "table",
    "bad argument #1 to 'getmetatable' (table expected, got " .. type_t .. ")"
  )
  return lua_getmetatable(t)
end

-- Manually removed, since this function can be accessed through a string's metatable
string.dump = nil

function debug_fmt(v)
  local vt = type(v)
  if vt == "function" or vt == "table" or vt == "thread" or vt == "userdata" then
    local s = tostring(v)
    if string.find(s, "^" .. vt) then
      return "<" .. s .. ">"
    else
      return bstr_debug_fmt(s)
    end
  elseif vt == "string" then
    return bstr_debug_fmt(v)
  else
    return tostring(v)
  end
end
