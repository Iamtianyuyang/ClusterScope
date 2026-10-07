# Gauntlet.cmake -- include from your top-level CMakeLists.txt:
#
#   include(.gauntlet/cmake/Gauntlet.cmake)
#   ...define your library targets...
#   gauntlet_add_acceptance(LINK my_core_lib)   # builds acceptance/steps/*.cpp + generated scenarios
#
# Coverage instrumentation is enabled only for the gauntlet build (-DGAUNTLET_COVERAGE=ON),
# which `node .gauntlet/gauntlet.mjs build` passes. Your normal builds are untouched.

option(GAUNTLET_COVERAGE "Instrument with clang source-based coverage (used by the gauntlet)" OFF)
set(GAUNTLET_KIT_DIR "${CMAKE_CURRENT_LIST_DIR}/.." CACHE INTERNAL "")

if(GAUNTLET_COVERAGE)
  if(NOT CMAKE_CXX_COMPILER_ID MATCHES "Clang")
    message(FATAL_ERROR "GAUNTLET_COVERAGE requires clang (got ${CMAKE_CXX_COMPILER_ID}). "
                        "Configure with -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++.")
  endif()
  # Only C/C++ translation units get clang flags: nvcc (CUDA) must never see them.
  add_compile_options("$<$<COMPILE_LANGUAGE:C,CXX>:-fprofile-instr-generate;-fcoverage-mapping;-O0;-g>")
  add_link_options("$<$<LINK_LANGUAGE:C,CXX>:-fprofile-instr-generate>")
  # A target linked by nvcc that contains instrumented C/C++ objects needs the clang profile runtime:
  # pass the flag through nvcc's host compiler (configure CMAKE_CUDA_HOST_COMPILER=clang++ for that).
  if(CMAKE_CUDA_HOST_COMPILER MATCHES "clang")
    add_link_options("$<$<LINK_LANGUAGE:CUDA>:-Xcompiler=-fprofile-instr-generate>")
  endif()
endif()

# Sanitizer build (-DGAUNTLET_SANITIZE=ON, a separate build dir used by `gauntlet sanitize`):
# AddressSanitizer + UndefinedBehaviorSanitizer on every C/C++ translation unit; CUDA device code is
# checked at run time by compute-sanitizer instead.
option(GAUNTLET_SANITIZE "Build C/C++ with AddressSanitizer + UndefinedBehaviorSanitizer (used by the gauntlet)" OFF)
if(GAUNTLET_SANITIZE)
  set(_gauntlet_san "-fsanitize=address,undefined;-fno-sanitize-recover=undefined;-fno-omit-frame-pointer;-g;-O1")
  add_compile_options("$<$<COMPILE_LANGUAGE:C,CXX>:${_gauntlet_san}>")
  add_link_options("$<$<LINK_LANGUAGE:C,CXX>:-fsanitize=address,undefined>")
  if(WIN32)
    # MSVC ABI: link the DLL sanitizer runtime to match the DLL CRT (the static one is built for /MT)
    add_link_options("$<$<LINK_LANGUAGE:C,CXX>:-shared-libsan>")
  endif()
  if(CMAKE_CUDA_HOST_COMPILER MATCHES "clang")
    add_link_options("$<$<LINK_LANGUAGE:CUDA>:-Xcompiler=-fsanitize=address,undefined>")
  endif()
endif()

enable_testing()

function(gauntlet_add_acceptance)
  cmake_parse_arguments(GA "" "STEPS_DIR;GENERATED_DIR" "LINK" ${ARGN})
  if(NOT GA_STEPS_DIR)
    set(GA_STEPS_DIR "${CMAKE_SOURCE_DIR}/acceptance/steps")
  endif()
  if(NOT GA_GENERATED_DIR)
    set(GA_GENERATED_DIR "${CMAKE_SOURCE_DIR}/acceptance/generated")
  endif()
  if(NOT EXISTS "${GA_GENERATED_DIR}/acceptance_main.cpp")
    message(WARNING "gauntlet: ${GA_GENERATED_DIR}/acceptance_main.cpp missing -- run `node .gauntlet/gauntlet.mjs gen`")
    return()
  endif()
  file(GLOB_RECURSE _gauntlet_steps CONFIGURE_DEPENDS "${GA_STEPS_DIR}/*.cpp" "${GA_STEPS_DIR}/*.cc")
  add_executable(gauntlet_acceptance "${GA_GENERATED_DIR}/acceptance_main.cpp" ${_gauntlet_steps})
  target_compile_features(gauntlet_acceptance PRIVATE cxx_std_17)
  target_include_directories(gauntlet_acceptance PRIVATE "${GAUNTLET_KIT_DIR}/runtime")
  if(GA_LINK)
    target_link_libraries(gauntlet_acceptance PRIVATE ${GA_LINK})
  endif()
  # Re-run CMake when the generated test list changes.
  set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${GA_GENERATED_DIR}/acceptance_tests.cmake")
  include("${GA_GENERATED_DIR}/acceptance_tests.cmake")
endfunction()
