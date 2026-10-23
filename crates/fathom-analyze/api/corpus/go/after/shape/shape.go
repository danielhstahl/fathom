// Package shape is the fixture package — after the episode.
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
	Label string
	label string
}

func NewRect(w, h float64, label string) *Rect {
	return &Rect{W: w, H: h, Label: label}
}

func (r Rect) Area() float64 {
	return r.W * r.H
}

// Deprecated: Name is being replaced by Label.
func (r *Rect) Name() string {
	return "rect"
}

func Distance(a, b float64, eps ...float64) float64 {
	return a - b
}

func Perimeter(w, h float64) float64 {
	return 2 * (w + h)
}

const Sides = 5

var Version = "1.1.0"

func helper(x int) int {
	return x + 2
}
