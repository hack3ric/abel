local bstr_debug_fmt = ...

local function table_merge(a, b)
  local result = {}
  for k, v in pairs(a) do
    result[k] = v
  end
  for k, v in pairs(b) do
    result[k] = v
  end
  return result
end

function HttpError(obj)
  local status, error, detail = obj.status, obj.error, obj.detail

  local type_detail = type(detail)
  local default_detail, detail_fn
  if type_detail == "function" then
    detail_fn = detail
  elseif type_detail == "table" then
    default_detail = detail
    detail_fn = function(new_detail)
      return table_merge(detail, new_detail)
    end
  elseif type_detail == "nil" then
    detail_fn = function(detail)
      return detail
    end
  else
    default_detail = { msg = tostring(detail) }
    detail_fn = function(new_detail)
      return table_merge(default_detail, new_detail)
    end
  end

  return setmetatable({
    status = status,
    error = error,
    detail = default_detail,
  }, {
    __call = function(self, ...)
      return {
        status = status,
        error = error,
        detail = detail_fn(...),
      }
    end
  })
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
