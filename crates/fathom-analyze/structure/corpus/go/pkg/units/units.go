// Package units is imported with a dot import by main, so every exported name of this
// package lands in the importer's file scope.
package units

const Km = 1000

var Table = map[string]float64{"km": Km}

func Meters(v float64) float64 {
	return v * Km
}

func normalize(v float64) float64 {
	return v
}
