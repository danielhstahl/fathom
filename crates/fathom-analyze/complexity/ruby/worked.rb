# ADR-007 §4.5's list. Ruby spells a test in more shapes than the other five put together, and the
# modifier forms are the ones a by-eye count misses.

class Shape
  attr_accessor :name

  define_method(:perimeter) do
    if @name
      1
    else
      0
    end
  end

  def initialize(name, sides)
    @name = name
    @sides = sides
  end

  def describe
    if @sides.zero?
      'none'
    elsif @sides == 1
      'dot'
    else
      unless @name.nil?
        return "shape:#{@name}"
      end
      'shape'
    end
  end

  def short?
    @sides < 2
  end

  def label
    "sides:#{@sides}" if @sides
  end

  def risky
    value = @name.fetch(:missing) rescue 'fallback'
    value
  end

  def kind
    case @sides
    when 0, 1
      'few'
    when 2
      'two'
    else
      'many'
    end
  end

  def matched(command)
    case command
    in [String => action]
      action
    in [Integer => count] if count.positive?
      'counted'
    in [_, *rest]
      rest.to_s
    end
  end

  def each(rows)
    for row in rows
      next if row.nil?

      yield row
    end
    index = 0
    while index < rows.size
      index += 1
    end
    until rows.empty?
      rows.pop
    end
    rows.each { |row| puts row if row }
  end

  def self.build(spec)
    return nil if spec.nil?

    parts = spec.split(':')
    parts.size > 1 ? Shape.new(parts[0], parts[1].to_i) : Shape.new(spec, 0)
  end
end

class Extras
end

# A reopened class this file does not declare: the methods written here are not in this file's
# declaration list, which is what `monkeypatch` marks (ADR-007 §7.3).
Extras.class_eval do
  def extra
    raise 'no' if @missing

    :extra
  end
end

class Computed
  # Names computed at run time: `s1` found 4 of 4 of these missed and called it irreducible, and the
  # mark is the answer rather than a missing row.
  %i[a b c].each do |name|
    define_method(name) { name.to_s }
  end
end
