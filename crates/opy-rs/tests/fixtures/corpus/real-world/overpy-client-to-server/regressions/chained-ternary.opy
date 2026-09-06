rule "chained ternary regression":
    @Event global
    debug(1 if true else 2 if false else 3)
