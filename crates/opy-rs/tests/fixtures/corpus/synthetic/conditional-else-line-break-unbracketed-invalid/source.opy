globalvar A
globalvar B

rule "else line break outside brackets":
    @Event global
    A = 1 if B >= 3 else
        2
