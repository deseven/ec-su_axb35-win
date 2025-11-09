EnableExplicit
Define ev.i
#myName = "ec-su_axb35-win PoC"

;IconName$ = #PB_Compiler_Home + "examples/sources/Data/CdPlayer.ico"

Enumeration #PB_Event_FirstCustomValue
  #gotVersion
  #gotTemp
  #gotRPMs
EndEnumeration

Enumeration
  #window
  #verLabel
  #ver
  #pwrLabel
  #pwr
  #tempLabel
  #temp
  #fan1ModeLabel
  #fan1Mode
  #fan1RPMLabel
  #fan1RPM
  #fan2ModeLabel
  #fan2Mode
  #fan2RPMLabel
  #fan2RPM
  #fan3ModeLabel
  #fan3Mode
  #fan3RPMLabel
  #fan3RPM
EndEnumeration

Define lastCheck.i
Define ecFWver.s
Define temp.l
Define.l fan1rpm,fan2rpm,fan3rpm

Procedure ecRead(interval.i)
  Protected lastRead.i,currentTime.i
  Shared ecFWver.s,temp.l
  Shared fan1rpm,fan2rpm,fan3rpm
  Repeat
    currentTime = ElapsedMilliseconds()
    If (currentTime - lastRead >= interval) Or (lastRead = 0)
      Protected NewList output.s()
      Protected probe = RunProgram("C:\Program Files (x86)\NoteBook FanControl\ec-probe.exe","dump","",#PB_Program_Open|#PB_Program_Read|#PB_Program_Hide)
      While ProgramRunning(probe)
        If AvailableProgramOutput(probe)
          AddElement(output())
          output() = ReadProgramString(probe)
        EndIf
        Delay(3)
      Wend
      CloseProgram(probe)
      ForEach output()
        Select ListIndex(output())
          Case 2 ; version info
            If Not Len(ecFWver)
              ecFWver = Str(Val("$" + StringField(output(),3," "))) + ".0" + Str(Val("$" + StringField(output(),4," ")))
              PostEvent(#gotVersion)
            EndIf
          Case 4 ; fan3 rpm
            fan3rpm = Val("$" + StringField(output(),11," ") + StringField(output(),12," "))
          Case 5 ; fans 1-2 rpm
            fan1rpm = Val("$" + StringField(output(),8," ") + StringField(output(),9," "))
            fan2rpm = Val("$" + StringField(output(),10," ") + StringField(output(),11," "))
            PostEvent(#gotRPMs)
          Case 9 ; temp
            temp = Val("$" + StringField(output(),3," "))
            PostEvent(#gotTemp)
        EndSelect
      Next
      FreeList(output())
      lastRead = currentTime
    EndIf
    Delay(100)
  ForEver
EndProcedure

OpenWindow(#window,#PB_Ignore,#PB_Ignore,200,300,#myName,#PB_Window_SystemMenu|#PB_Window_ScreenCentered)
StickyWindow(#window,#True)
Define offsetX = 20
TextGadget(#verLabel,10,10,100,20,"EC FW Version:")
TextGadget(#ver,110,10,100,20,"N/A")

TextGadget(#pwrLabel,10,10 + (offsetX*2),100,20,"Power Mode:")
TextGadget(#pwr,110,10 + (offsetX*2),100,20,"N/A")
TextGadget(#tempLabel,10,10 + (offsetX*3),100,20,"APU Temperature:")
TextGadget(#temp,110,10 + (offsetX*3),100,20,"N/A")

TextGadget(#fan1ModeLabel,10,10 + (offsetX*5),100,20,"Fan1 Mode:")
TextGadget(#fan1Mode,110,10 + (offsetX*5),100,20,"N/A")
TextGadget(#fan1RPMLabel,10,10 + (offsetX*6),100,20,"Fan1 RPM:")
TextGadget(#fan1RPM,110,10 + (offsetX*6),100,20,"N/A")

TextGadget(#fan2ModeLabel,10,10 + (offsetX*7),100,20,"Fan2 Mode:")
TextGadget(#fan2Mode,110,10 + (offsetX*7),100,20,"N/A")
TextGadget(#fan2RPMLabel,10,10 + (offsetX*8),100,20,"Fan2 RPM:")
TextGadget(#fan2RPM,110,10 + (offsetX*8),100,20,"N/A")

TextGadget(#fan3ModeLabel,10,10 + (offsetX*9),100,20,"Fan3 Mode:")
TextGadget(#fan3Mode,110,10 + (offsetX*9),100,20,"N/A")
TextGadget(#fan3RPMLabel,10,10 + (offsetX*10),100,20,"Fan3 RPM:")
TextGadget(#fan3RPM,110,10 + (offsetX*10),100,20,"N/A")

Define readTh.i = CreateThread(@ecRead(),1000)

Repeat
  ev = WaitWindowEvent(1000)
  Select ev
    Case #gotVersion
      SetGadgetText(#ver,ecFWver)
    Case #gotTemp
      SetGadgetText(#temp,Str(temp) + " °C")
    Case #gotRPMs
      SetGadgetText(#fan1RPM,Str(fan1rpm))
      SetGadgetText(#fan2RPM,Str(fan2rpm))
      SetGadgetText(#fan3RPM,Str(fan3rpm))
  EndSelect
Until ev = #PB_Event_CloseWindow
; IDE Options = PureBasic 6.21 (Windows - x64)
; CursorPosition = 81
; FirstLine = 72
; Folding = -
; EnableXP
; DPIAware