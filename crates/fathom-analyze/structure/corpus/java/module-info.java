module com.acme.app {
    requires java.base;

    exports com.acme.app;
    exports com.acme.app.model;
    exports com.acme.app.util;
    // com.acme.app.legacy is deliberately not exported to other modules.
}
