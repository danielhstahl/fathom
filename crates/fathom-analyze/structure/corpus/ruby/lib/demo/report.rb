# frozen_string_literal: true

require_relative "core/circle"

module Demo
  class Report
    FIELDS = %i[name area].freeze

    def initialize(items)
      @items = items
    end

    FIELDS.each do |field|
      define_method(field) do
        @items.map { |item| item.public_send(field) }
      end
    end

    def self.header(*names)
      names.join(" | ")
    end

    def self.from_file(path)
      new([])
    end

    def to_s
      text = <<~TEXT
        report
        #{self.class.header(*FIELDS.map(&:to_s))}
      TEXT
      text
    end

    def method_missing(name, *args)
      nil
    end

    def respond_to_missing?(name, include_private = false)
      true
    end
  end
end
