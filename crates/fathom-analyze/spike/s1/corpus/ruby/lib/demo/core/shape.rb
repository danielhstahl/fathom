# frozen_string_literal: true

module Demo::Core
  module Mixin
    def tagged
      "#{self.class}:#{name}"
    end
  end

  class Shape
    include Mixin

    attr_reader :id
    attr_accessor :name, :label

    Unit = "m"

    METHODS = %w[side1 side2].freeze

    class << self
      def registered
        @registered ||= {}
      end

      def register(klass)
        registered[klass.name] = klass
      end
    end

    def initialize(name, id: 0)
      @name = name
      @id = id
    end

    def area
      raise NotImplementedError
    end

    def <=>(other)
      area <=> other.area
    end

    def ==(other)
      other.respond_to?(:area) && area == other.area
    end

    def describe
      "#{name}:#{area}"
    end

    define_method(:perimeter) { 0.0 }

    alias perim perimeter

    METHODS.each do |method_name|
      define_method(method_name) { method_name.to_sym }
    end

    private def helper
      42
    end

    protected def shield
      :shielded
    end
  end

  class Base < Shape
    def area
      0.0
    end
  end

  Pointy = Struct.new(:x, :y) do
    def norm
      Math.sqrt(x * x + y * y)
    end
  end
end
