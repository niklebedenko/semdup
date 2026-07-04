# Derived from sinatra (lib/sinatra/base.rb)
# at 7b50a1bbb5324838908dfaa00ec53ad322673a29, MIT licensed.
# Planted-clone eval asset for semdup; not production code.
# Same behavior as Sinatra's #uri helper, restructured: the scheme and
# authority are built as separate values and joined once at the end.

module UrlBuilding
  def full_url(path = nil, with_authority = true, with_script = true)
    return path if path.to_s =~ /\A[a-z][a-z0-9+.\-]*:/i

    pieces = []
    if with_authority
      scheme = request.secure? ? 'https' : 'http'
      default_port = request.secure? ? 443 : 80
      authority =
        if request.forwarded? || request.port != default_port
          request.host_with_port
        else
          request.host
        end
      pieces << "#{scheme}://#{authority}"
    else
      # Keep a leading empty segment so the result stays root-relative.
      pieces << ''
    end
    pieces << request.script_name.to_s if with_script
    pieces << (path || request.path_info).to_s
    File.join(pieces)
  end
end
