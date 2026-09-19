globalvar a

rule "random outer delete":
    @Event global
    del a[random.randint(0, 1)][0][0]
