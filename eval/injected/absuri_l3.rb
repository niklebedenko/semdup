# Derived from sinatra (lib/sinatra/base.rb)
# at 7b50a1bbb5324838908dfaa00ec53ad322673a29, MIT licensed.
# Planted-clone eval asset for semdup; not production code.
# Spec: Return the argument untouched when it already carries a URI scheme
# (a letter followed by letters, digits, "+", "." or "-", then ":").
# Otherwise build a URL for the current request: optionally prefixed by
# scheme and authority (the explicit host:port when the request was
# forwarded or uses a non-default port, the bare host otherwise), optionally
# followed by the script name, then the given path (falling back to the
# request's path info). Segments join with exactly one slash between them;
# without the authority prefix the result is root-relative.

module LinkTarget
  def expand_link(dest = nil, include_origin = true, include_script = true)
    return dest if dest.to_s.match?(/\A[a-z][a-z0-9+.\-]*:/i)

    segments = []
    segments <<
      if include_origin
        scheme = request.secure? ? 'https' : 'http'
        nonstandard = request.port != (request.secure? ? 443 : 80)
        host = request.forwarded? || nonstandard ? request.host_with_port : request.host
        "#{scheme}://#{host}"
      else
        ''
      end
    segments << request.script_name.to_s if include_script
    segments << (dest || request.path_info).to_s
    File.join(*segments)
  end
end
