# Derived from sinatra (lib/sinatra/base.rb)
# at 7b50a1bbb5324838908dfaa00ec53ad322673a29, MIT licensed.
# Planted-clone eval asset for semdup; not production code.
# Spec: Assemble a Cache-Control response header. Positional arguments are
# directive names; a trailing hash may add more: a value of true becomes a
# bare directive, false is dropped, and anything else becomes name=value
# (max-age and s-maxage values are coerced to integers). Underscores in
# directive names become dashes. The header is only set when at least one
# directive survives.

module ResponseFreshness
  def freshness(*args)
    extra = args.last.is_a?(Hash) ? args.pop : {}

    directives = args.map { |a| a.to_s.tr('_', '-') }
    extra.each_pair do |k, v|
      next if v == false

      label = k.to_s.tr('_', '-')
      if v == true
        directives << label
      else
        v = v.to_i if label == 'max-age' || label == 's-maxage'
        directives << "#{label}=#{v}"
      end
    end

    response['Cache-Control'] = directives.join(', ') unless directives.empty?
  end
end
