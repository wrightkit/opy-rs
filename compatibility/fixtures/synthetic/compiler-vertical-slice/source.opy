globalvar A
rule "issue 35 integration":
    @Event global
    A = 1
    disableInspector()
