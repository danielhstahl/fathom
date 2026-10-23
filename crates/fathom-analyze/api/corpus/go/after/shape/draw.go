package shape

import "fmt"

// Draw moved file inside the same package, which is not a moved name in Go.
func Draw(target any) string {
	return fmt.Sprint(target)
}
