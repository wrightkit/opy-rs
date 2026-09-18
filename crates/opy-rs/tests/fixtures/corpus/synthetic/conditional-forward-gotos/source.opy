globalvar state

rule "conditional goto":
    @Event global
    if state == 1:
        goto done
    state = 2
    done:
    state = 3

rule "nested conditional goto":
    @Event global
    if state == 1:
        if state == 2:
            goto nested_done
    state = 4
    nested_done:
    state = 5

rule "conditional goto with else":
    @Event global
    if state == 1:
        goto else_done
    else:
        state = 6
    state = 7
    else_done:
    state = 8

rule "conditional goto after mutation":
    @Event global
    if state == 1:
        state = 2
        goto mutated_done
    state = 3
    mutated_done:
    state = 4

rule "leading goto with tail":
    @Event global
    if state == 1:
        goto leading_done
        state = 9
    leading_done:
    state = 10
