// ADR-007 §4.6's list. Two of Java's branch spellings are the ones a by-eye count double-counts: a
// classic `case 1: case 2:` group is one arm, and an arrow `case A, B ->` is one rule.
package demo;

import java.io.IOException;
import java.io.Reader;
import java.io.StringReader;

public interface Shape {
    // No body here: a unit with no figure, which prints `?` and never `0`.
    double area();

    default String name() {
        return "shape";
    }
}

abstract class Base implements Shape {
    private final double radius;

    Base(double radius) {
        this.radius = radius;
    }

    abstract double scaled();

    Base() {
        this(1.0);
    }

    String kind(Object value) {
        switch (value) {
            case Integer when radius > 0 -> {
                return "guarded";
            }
            case Integer i -> {
                return "int";
            }
            default -> {
                return "other";
            }
        }
    }

    int classic(int v) {
        switch (v) {
            case 1:
            case 2:
                return 2;
            default:
                return 0;
        }
    }

    double total(double[] rows, boolean round) throws IOException {
        double total = 0;
        for (double row : rows) {
            if (row < 0 || Double.isNaN(row)) {
                continue;
            }
            total += row;
        }
        for (int index = 0; index < rows.length; index++) {
            total += index;
        }
        while (total > 1000) {
            total -= 1000;
        }
        do {
            total -= 1;
        } while (total < 0);
        try (Reader reader = new StringReader("x")) {
            if (reader.read() < 0 && round) {
                total = 0;
            }
        } catch (IOException | RuntimeException error) {
            throw error;
        } finally {
            total = Math.abs(total);
        }
        return value instanceof Number number ? number.doubleValue() : total;
    }

    long viaValue(Object value) {
        if (value instanceof Number) {
            return ((Number) value).longValue();
        }
        return 0L;
    }

    java.util.function.IntUnaryOperator lambda() {
        return value -> value > 0 ? value : -value;
    }
}

record Circle(String name, double radius) implements Shape {
    // A compact constructor: a unit of its own, and the record's accessors are the compiler's, so
    // the record's scope is marked `synthesised_accessors` and every figure beside it is a floor.
    Circle {
        if (radius < 0 || name.isEmpty()) {
            throw new IllegalArgumentException("bad");
        }
    }

    public double area() {
        return Math.PI * radius * radius;
    }

    public double scaled() {
        return area() * 2;
    }
}
