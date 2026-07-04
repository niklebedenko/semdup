# Derived from sinatra (lib/sinatra/base.rb)
# at 7b50a1bbb5324838908dfaa00ec53ad322673a29, MIT licensed.
# Planted-clone eval asset for semdup; not production code.

module AddressHelpers
  def absolute_address(target = nil, absolute = true, prepend_script = true)
    return target if target.to_s =~ /\A[a-z][a-z0-9+.\-]*:/i

    parts = [prefix = String.new]
    if absolute
      prefix << "http#{'s' if request.secure?}://"
      prefix << if request.forwarded? || (request.port != (request.secure? ? 443 : 80))
                  request.host_with_port
                else
                  request.host
                end
    end
    parts << request.script_name.to_s if prepend_script
    parts << (target || request.path_info).to_s
    File.join parts
  end
end
