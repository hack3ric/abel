--- @class http
local http = {}

--- @param req string | Uri | Request
--- @return Response
function http.request(req) end

--- @class Uri
--- @field scheme string
--- @field host string
--- @field port integer
--- @field authority string
--- @field path string
--- @field query_string string
local Uri = {}

--- @alias UriBuilder { scheme?: string, authority?: string, path_and_query?: string, path?: string, query?: string | table<string, string> }

--- @param uri string
--- @return Uri
--- @overload fun(builder: UriBuilder): Uri
function http.Uri(uri) end

--- @alias QueryMap table<string, QueryField>
--- @alias QueryField string | QueryMap | QueryField[]

--- @return QueryMap
function Uri:query() end

--- @alias Body nil | string | Value | ByteStream

--- @class HeaderMap

--- @class Request
--- @field method string
--- @field uri Uri
--- @field body Body
--- @field headers HeaderMap
--- @field params string[]

--- @class Response
--- @field status integer
--- @field body Body
--- @field headers HeaderMap 

--- @alias ResponseBuilder { status?: integer, headers?: table<string, string | string[]>, body?: Body }

--- @param builder ResponseBuilder
--- @return Response
function http.Response(builder) end

return http
