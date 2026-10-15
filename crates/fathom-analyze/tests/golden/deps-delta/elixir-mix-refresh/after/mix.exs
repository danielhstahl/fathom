defmodule App.MixProject do
  use Mix.Project

  def project do
    [app: :app, version: "0.1.0", deps: deps()]
  end

  defp deps do
    [
      {:ecto, "~> 3.11"},
      {:phoenix, "~> 1.7"},
      {:internal, git: "https://github.com/other/internal-fork", branch: "stable"}
    ]
  end
end
