package com.acme.app;

import com.acme.app.model.Circle;
import static com.acme.app.util.Geom.area;
import java.util.ArrayList;
import java.util.List;

@Tracked("app")
public final class App {
    public static final String VERSION = "1.0";

    private App() {}

    public static void main(String[] args) {
        Circle circle = new Circle("c", 2);
        List<String> names = new ArrayList<>();
        names.add(circle.name());
        System.out.printf("%s %n", area(circle));
    }

    public static <T extends Comparable<T>> T maxOf(T a, T b) {
        return a.compareTo(b) >= 0 ? a : b;
    }

    public Runnable task() {
        return () -> System.out.println("hi");
    }

    @Override
    public String toString() {
        return "App";
    }

    public static final class Helper {
        public int depth() {
            return 1;
        }

        static class Internal {
            private int secret() {
                return 0;
            }
        }
    }

    public interface Callback {
        void call(String s);

        default int arity() {
            return 1;
        }
    }

    public enum Mode {
        FAST,
        SLOW;

        public String label() {
            return name().toLowerCase();
        }
    }
}
