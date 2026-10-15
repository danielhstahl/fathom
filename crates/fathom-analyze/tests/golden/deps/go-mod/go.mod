module github.com/acme/api

go 1.22

toolchain go1.22.3

require (
	github.com/go-chi/chi/v5 v5.0.12
	github.com/lib/pq v1.10.9 // indirect
	golang.org/x/net v0.24.0
)

require github.com/stretchr/testify v1.9.0

replace github.com/lib/pq => ./forks/pq

replace (
	golang.org/x/net => github.com/acme/net v0.24.0
)

exclude github.com/stretchr/testify v1.8.0

tool github.com/golangci/golangci-lint/cmd/golangci-lint
