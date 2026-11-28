// The Rust library holds the app. winit starts UIKit from inside this call.
void isotherm_app_main(void);

int main(int argc, char *argv[]) {
    isotherm_app_main();
    return 0;
}
