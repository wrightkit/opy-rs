globalvar A

rule "inherited strict":
    @Event global
    A = "am" == "**"

#!disableOptimizeStrict
