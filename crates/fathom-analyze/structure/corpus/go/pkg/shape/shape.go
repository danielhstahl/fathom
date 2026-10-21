// Package shape is the corpus's main package: interfaces, generics, and a second file
// declaring the same package.
package shape

import "errors"

// Shape is the contract every figure keeps.
type Shape interface {
	Area() float64
	Name() string
}

// Visitor is a function type, which is a type definition like any other.
type Visitor func(Shape) error

type Options struct {
	Shape
	Prefix string
	Limit  int
}

type Rect struct {
	W, H float64
}

const (
	DefaultSides = 4
	maxSides     = 12
)

var ErrEmpty = errors.New("empty shape")

func NewRect(w, h float64) *Rect {
	return &Rect{W: w, H: h}
}

func (r Rect) Area() float64 {
	return r.W * r.H
}

func (r *Rect) Scale(f float64) {
	r.W *= f
	r.H *= f
}

func (r Rect) Name() string {
	return "rect"
}

func Map[T any, U any](items []T, fn func(T) U) []U {
	out := make([]U, 0, len(items))
	for _, item := range items {
		out = append(out, fn(item))
	}
	return out
}
