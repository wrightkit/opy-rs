globalvar total
globalvar missing

rule "negative primitive":
    @Event global
    total = {"a": 1, "b": 2}["a"]
    missing = {"a": 1, "b": 2}["c"]
