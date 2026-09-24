globalvar g
globalvar h
rule "x":
    @Event global
    for g in range(3):
        if h == 3:
            goto lbl_done
        h = 1
        wait(1)
    lbl_done:
    h = 9
