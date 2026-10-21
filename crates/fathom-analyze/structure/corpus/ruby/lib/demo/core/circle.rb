# frozen_string_literal: true

module Demo
  module Core
    class Circle < Shape
      include Comparable

      defdelegate :centroid, to: :box

      def initialize(name, radius: 1.0, id: 0)
        super(name, id: id)
        @radius = radius
      end

      def area
        Math::PI * @radius**2
      end

      def radius
        @radius
      end

      def self.unit(scale = 1)
        Core::Shape::Unit * scale
      end

      def to_h
        { name: name, area: area }
      end
    end
  end
end
