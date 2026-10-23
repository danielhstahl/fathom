package com.acme.pay;

/** A charge against an account. */
public class Payment {
    public final int amount;
    String memo;

    public Payment(int amount) {
        this.amount = amount;
    }

    public int amount() {
        return amount;
    }

    public void refund() {}

    void internal() {}
}

class Ledger {
    void post() {}
}
