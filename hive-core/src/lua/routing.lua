local function bind_one(f, arg)
  return function(...)
    return f(arg, ...)
  end
end

local function add_method_route(method, self, handler)
  self["$" .. method] = handler
  return self
end

local mt = {
  __index = {
    any = bind_one(add_method_route, "_"),
    get = bind_one(add_method_route, "GET"),
    post = bind_one(add_method_route, "POST"),
    put = bind_one(add_method_route, "PUT"),
    patch = bind_one(add_method_route, "PATCH"),
    head = bind_one(add_method_route, "HEAD"),
    delete = bind_one(add_method_route, "DELETE"),
    trace = bind_one(add_method_route, "TRACE"),
  },
  __call = function(self, req)
    local handler = self["$" .. req.method]
    local any = self["$_"]
    if type(handler) == "function" then
      return handler(req)
    elseif type(any) == "function" then
      return any(req)
    else
      local allowed_methods = {}
      for k, _ in pairs(self) do
        if k:sub(1, 1) == "$" then
          allowed_methods[#allowed_methods + 1] = k:sub(2)
        end
      end

      error {
        status = 405,
        error = "method not allowed",
        detail = {
          allowed_methods = allowed_methods
        }
      }
    end
  end,
}

local function init_method_route(method, handler)
  return setmetatable({
    ["$" .. method] = handler
  }, mt)
end

local routing = {
  any = bind_one(init_method_route, "_"),
  get = bind_one(init_method_route, "GET"),
  post = bind_one(init_method_route, "POST"),
  put = bind_one(init_method_route, "PUT"),
  patch = bind_one(init_method_route, "PATCH"),
  head = bind_one(init_method_route, "HEAD"),
  delete = bind_one(init_method_route, "DELETE"),
  trace = bind_one(init_method_route, "TRACE"),
}

return routing
