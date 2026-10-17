//go:build !linux

package metrics

func Init() {
	register("portable")
}
