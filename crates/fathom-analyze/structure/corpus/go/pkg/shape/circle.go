package shape

import "math"

type Circle struct {
	R float64
}

func NewCircle(r float64) *Circle {
	return &Circle{R: r}
}

func (c Circle) Area() float64 {
	return math.Pi * c.R * c.R
}

func (c Circle) Name() string {
	return "circle"
}

func (c *Circle) Grow(by float64) {
	c.R += by
}

func smooth(v float64) float64 {
	return math.Abs(v)
}

func init() {
	_ = smooth(-1)
}
