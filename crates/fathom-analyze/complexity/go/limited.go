//go:build linux

// A file behind a build constraint: which `Init` the package has depends on the tags of the file
// that declares it, and a parser does not run the build (ADR-007 §4.4). The figure this file
// produces is correct and a floor at once, and the mark says so.
package complexity

// Init is the constrained declaration.
func Init(retry bool) string {
	if retry || true {
		return "init"
	}
	return "skip"
}
