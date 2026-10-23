package com.acme.pay;

/** A charge against an account. */
public class Payment {
    public final int amount;
    String memo;

    public Payment(int amount) {
        this.amount = amount;
    }

    public int amountCents() {
        return amount;
    }

    public void refund(String reason) {}

    void internal() {}
}

public class Ledger {
    void post() {}
}
