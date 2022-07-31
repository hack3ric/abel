local stream = {}
local json_parse = ...

local function check_stream(st)
  local type_st = type(st)
  if type_st ~= "table" and type_st ~= "userdata" or not st.read then
    error("stream expected, got " .. type_st)
  end
end

local function check_sink(sink)
  local type_sink = type(sink)
  if type_sink ~= "table" and type_sink ~= "userdata" or not sink.write then
    error("sink expected, got " .. type_sink)
  end
end

local function check_transform(tr)
  local type_tr = type(tr)
  if type_tr ~= "table" and type_tr ~= "userdata" or not tr.transform then
    error("transform expected, got " .. type_tr)
  end
end

-- Only for items that are concatenatable, usually bytes
function stream.read_all(st)
  check_stream(st)
  local buf = st:read()
  for item in stream.iter(st) do
    buf = buf .. item
  end
  return buf
end

function stream.parse_json(st)
  check_stream(st)
  local str = stream.read_all(st)
  return json_parse(str)
end

function stream.iter(st)
  check_stream(st)
  return function() return st:read() end
end

function stream.from_iter(iter, state, ...)
  local var = { ... }
  return setmetatable({
    read = function(self)
      var = { iter(state, table.unpack(var)) }
      return table.unpack(var)
    end
  }, { __index = stream })
end

function stream.pipe_to(st, sink)
  check_stream(st)
  check_sink(sink)
  for item in stream.iter(st) do
    sink:write(item)
  end
end

function stream.pipe_through(st, tr)
  check_stream(st)
  check_transform(tr)
  return setmetatable({
    read = function(self)
      return tr:transform(st:read())
    end
  }, { __index = st })
end

function stream.recv_through(sink, tr)
  check_sink(sink)
  check_transform(tr)
  return setmetatable({
    write = function(self, item)
      sink:write(tr:transform(item))
    end
  }, { __index = sink })
end

return stream
