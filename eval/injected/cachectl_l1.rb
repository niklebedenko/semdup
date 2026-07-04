# Derived from sinatra (lib/sinatra/base.rb)
# at 7b50a1bbb5324838908dfaa00ec53ad322673a29, MIT licensed.
# Planted-clone eval asset for semdup; not production code.

module HttpCaching
  def cache_policy(*flags)
    if flags.last.is_a?(Hash)
      opts = flags.pop
      opts.reject! { |_name, setting| setting == false }
      opts.reject! { |name, setting| flags << name if setting == true }
    else
      opts = {}
    end

    flags.map! { |flag| flag.to_s.tr('_', '-') }
    opts.each do |name, setting|
      name = name.to_s.tr('_', '-')
      setting = setting.to_i if %w[max-age s-maxage].include? name
      flags << "#{name}=#{setting}"
    end

    response['Cache-Control'] = flags.join(', ') if flags.any?
  end
end
