// Command app is the entry point; it proves package import resolution and the two
// import forms that name no symbol at all.
package main

import (
	"fmt"

	"example.com/app/pkg/metrics"
	"example.com/app/pkg/shape"
	. "example.com/app/pkg/units"
	_ "example.com/app/pkg/version"
)

var version = "dev"

const commit = "abcdef1"

func init() {
	metrics.Init()
}

func main() {
	r := shape.NewRect(2, 3)
	fmt.Println(r.Area(), Meters(2), version, commit)
}

func banner(wide bool) string {
	if wide {
		return "wide " + version
	}
	return version
}
