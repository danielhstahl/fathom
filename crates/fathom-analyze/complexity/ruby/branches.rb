# frozen_string_literal: true

# Every Ruby spelling of a test that `worked.rb` does not carry: the four modifier forms, the two
# short-circuit operators and their low-precedence twins, safe navigation, and an `unless` guard on a
# `case`/`in` arm.
#
# This file exists because C-KIND (ADR-007 §4) refuses a decision-point list whose kinds no corpus
# file produces — "a kind nobody exercises is a kind whose counting code has never run" — and the
# first pass of this corpus had nine Ruby kinds that nothing had ever drawn out of the grammar. Ruby
# is the language that spells a test the most ways, so it is the language where a list copied from a
# reference page is likeliest to name a spelling the grammar does not use. Each method below earns
# exactly one thing being proved, and the figures next to them are the figures a reader gets by
# counting the same constructs the ADR lists in §4.5.
module Modifiers
  # A modifier is still a test (§3.4). Four one-line methods, four points, four figures of 2.
  def self.trim(value)
    return value if value.nil?
  end

  def self.skip(value)
    @last = value unless value.nil?
    nil
  end

  def self.consume(stream)
    line = stream.gets while stream
    line
  end

  def self.drain(queue)
    item = queue.pop until queue.empty?
    item
  end

  # `&&` and `||`, and their low-precedence twins `and` and `or`: four spellings of two tests. All
  # four are counted, because "a reviewer counts the tests they can see" is the rule that decides it
  # (§4.5), and a line of `a and b` shows a reviewer two things to check even though it is one node.
  def self.either(left, right)
    return :both if left && right

    :none
  end

  def self.neither(left, right)
    return :none if left || right

    :both
  end

  # `and`/`or` bind loose enough to be dangerous in a modifier position, so they are shown in the
  # head of an `if`, where the parse is not in doubt: two tests, one `if` node and one operator.
  def self.loose(left, right)
    if left and right
      :both
    else
      :none
    end
  end

  def self.anyway(left, right)
    if left or right
      :none
    else
      :both
    end
  end

  # Safe navigation is a test wearing a call: `value&.length` is `value.nil? ? nil : value.length`
  # (§4.5). In this grammar `&.` is an anonymous token, not a node, which is why it is a token rule.
  def self.length_of(value)
    value&.length
  end

  # A guard on a `case`/`in` arm is a second test on that arm; `unless` is the same thing written the
  # other way round, and `tree-sitter-ruby` gives it its own kind for the same reason.
  def self.only(value)
    case value
    in Integer unless value.zero?
      :nonzero
    else
      :other
    end
  end
end
