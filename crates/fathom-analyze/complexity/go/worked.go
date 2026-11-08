// Package complexity is ADR-007 §4.4's list in Go's own shapes: the one loop, the arms that are the
// branches, and the `if` whose initialisation statement carries the test.
package complexity

import (
	"errors"
	"fmt"
)

// Shape is a receiver for the method declarations below.
type Shape struct {
	Radius float64
	Kind   string
}

// Area is a method with the two shapes of a Go test: the plain one and the `v, ok` one.
func (s Shape) Area(kind string) string {
	if v, ok := s.Radius, s.Radius > 0; ok && v > 0 {
		return fmt.Sprint("round")
	} else if s.Kind != "" || kind != "" {
		return s.Kind
	}
	switch kind {
	case "small", "tiny":
		return "small"
	case "big":
		return "big"
	default:
		return "none"
	}
}

// Route is a type switch and a select: the arms are the branches, the headers are not.
func Route(value any, in chan string, out chan string) string {
	switch v := value.(type) {
	case int:
		if v > 0 {
			return "positive"
		}
	case string:
		if v == "" {
			return "empty"
		}
	default:
		return "other"
	}
	select {
	case got := <-in:
		if got != "" {
			return got
		}
	case out <- "ping":
		return "sent"
	default:
		return "idle"
	}
}

// Loops is Go's one loop in each of its four spellings, plus the exits that cost nothing.
func Loops(rows []int, limit int) (int, error) {
	total := 0
	for _, row := range rows {
		if row < 0 {
			continue
		}
		if row > limit {
			break
		}
		total += row
	}
	for index := 0; index < limit; index++ {
		total += index
	}
	for total > 1_000 {
		total -= 1_000
	}
	for {
		defer func() { total++ }()
		if total > 0 {
			return total, nil
		}
		return 0, errors.New("empty")
	}
}

// Extern has no body in this file — it is implemented in assembly, which is `body_not_visible` and
// not a figure of zero.
func Extern(x float64) float64
