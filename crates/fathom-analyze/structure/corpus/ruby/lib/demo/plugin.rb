# frozen_string_literal: true

require "json"
require_relative "core/shape"

module Demo
  module Plugin
    HANDLERS = {}

    def self.install(handlers)
      HANDLERS.merge!(handlers)
    end
  end
end

Demo::Core::Shape.class_eval do
  def extra
    :extra
  end

  def self.audit
    :audited
  end
end

Slate = Struct.new(:kind)

Demo::Plugin::HANDLERS[:shape] = Demo::Core::Shape

def standalone
  Slate.new(:plain)
end

if ENV["DEMO_LAZY"]
  require_relative "report"
end
