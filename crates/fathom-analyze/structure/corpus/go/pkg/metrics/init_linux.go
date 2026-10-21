//go:build linux

package metrics

func Init() {
	register("linux")
}

func register(kind string) {}
