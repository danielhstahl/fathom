package shape_test

import (
	"testing"

	"example.com/app/pkg/shape"
)

type fake struct {
	n int
}

func (f fake) Area() float64 { return float64(f.n) }

func (f fake) Name() string { return "fake" }

func TestArea(t *testing.T) {
	r := shape.NewRect(2, 2)
	if r.Area() != 4 {
		t.Fatal("bad area")
	}
}

func BenchmarkArea(b *testing.B) {
	for i := 0; i < b.N; i++ {
		_ = fake{n: i}.Area()
	}
}
