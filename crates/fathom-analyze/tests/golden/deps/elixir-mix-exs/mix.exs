defmodule Acme.MixProject do
  use Mix.Project

  def project do
    [
      app: :acme,
      version: "1.2.3",
      deps: deps()
    ]
  end

  defp deps do
    [
      {:phoenix, "~> 1.7.10"},
      {:jason, "~> 1.4", only: [:dev, :test]},
      {:ecto, "~> 3.11", optional: true},
      {:internal, github: "acme/internal", branch: "stable"},
      {:local, path: "../local", override: true}
    ]
  end
end
