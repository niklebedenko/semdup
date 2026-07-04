# Derived from sinatra (lib/sinatra/base.rb)
# at 7b50a1bbb5324838908dfaa00ec53ad322673a29, MIT licensed.
# Planted-clone eval asset for semdup; not production code.
# Same behavior as Sinatra's #cache_control, restructured: the option hash is
# partitioned into boolean toggles and valued directives instead of being
# mutated in place.

module CacheDirectives
  def emit_cache_header(*directives)
    options = directives.last.is_a?(Hash) ? directives.pop : {}

    toggles, valued = options.reject { |_, v| v == false }.partition { |_, v| v == true }
    directives.concat(toggles.map(&:first))

    tokens = directives.map { |d| d.to_s.tr('_', '-') }
    valued.each do |key, value|
      key = key.to_s.tr('_', '-')
      value = value.to_i if %w[max-age s-maxage].include?(key)
      tokens << "#{key}=#{value}"
    end

    response['Cache-Control'] = tokens.join(', ') unless tokens.empty?
  end
end
