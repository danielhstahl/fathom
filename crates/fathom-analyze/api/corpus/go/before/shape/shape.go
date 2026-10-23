// Package shape is the fixture package: export is a capital first rune, and the package is the unit.
package shape

import "fmt"

// Shape is the contract every figure keeps.
type Shape interface {
	Area() float64
	Name() string
}

// Rect is a rectangle.
type Rect struct {
	W float64
	H float64
	label string
}

func NewRect(w, h float64) *Rect {
	return &Rect{W: w, H: h}
}

func (r *Rect) Area() float64 {
	return r.W * r.H
}

func (r *Rect) Name() string {
	return "rect"
}

func Distance(a, b float64) float64 {
	return a - b
}

func Draw(target Shape) string {
	return fmt.Sprint(target)
}

func Gone() int {
	return 7
}

const Sides = 4

var Version = "1.0.0"

func helper(x int) int {
	return x + 1
}
