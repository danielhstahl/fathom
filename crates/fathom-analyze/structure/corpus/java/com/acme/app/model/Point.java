package com.acme.app.model;

public final class Point {
    public final double x;
    public final double y;

    public Point(double x, double y) {
        this.x = x;
        this.y = y;
    }

    public double norm() {
        return Math.sqrt(x * x + y * y);
    }

    public static Point origin() {
        return new Point(0, 0);
    }
}
