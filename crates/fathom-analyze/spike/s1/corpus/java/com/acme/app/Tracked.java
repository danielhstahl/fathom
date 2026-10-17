package com.acme.app;

public @interface Tracked {
    String value();

    int level() default 0;
}
