# The fixture gem's entry point. In Ruby the load graph is the surface, because a file can be
# loaded and thrown away at import time and no file-level scan can tell (`e3.7`, `reco-p5u`).
#
#   `require "demo"` loads exactly the files listed here.

require_relative "demo/version"
require_relative "demo/shape"
