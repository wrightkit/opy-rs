globalvar total

rule "negative primitive":
    @Event global
    total = {"a": 1, "b": 2}["a"]
