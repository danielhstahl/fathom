# frozen_string_literal: true

require "demo/version"
require "demo/report"
require "json"
require_relative "demo/core/shape"

module Demo
  Config = Struct.new(:verbose, :depth)

  DEFAULTS = {
    "unit" => "m",
    "depth" => 2
  }.freeze

  autoload :Circle, "demo/core/circle"

  class Error < StandardError
  end

  class << self
    def configure
      yield config
      config
    end

    def config
      @config ||= Config.new(false, 1)
    end
  end

  def self.reset!
    @config = nil
  end

  def self.parsed(raw)
    JSON.parse(raw)
  end
end
