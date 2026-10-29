macro p():
    eventPlayer
macro nested():
    p()
globalvar target = nested()
