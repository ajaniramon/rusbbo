{% set title = locale.hk_crear_usuario %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="grid g-side">
  <div class="card">
    <div class="card-head"><h2>{{ locale.hk_nueva_cuenta|raw }}</h2></div>
    <div class="card-body">
      <form class="form" method="post" action="{{ hk.path }}/users/create">
        <input type="hidden" name="hk_token" value="{{ hk.token }}">
        <div class="row r-2">
          <div class="field"><label>{{ locale.hk_nombre_de_usuario|raw }}</label><input type="text" name="username" maxlength="20" required value="{{ form.username }}"><span class="help">{{ locale.hk_3_20_caracteres_letras_numeros_y|raw }}</span></div>
          <div class="field"><label>{{ locale.hk_email_opcional|raw }}</label><input type="text" name="email" value="{{ form.email }}"></div>
        </div>
        <div class="row r-2">
          <div class="field"><label>{{ locale.hk_contrasena|raw }}</label><input type="password" name="password" minlength="6" required autocomplete="new-password"></div>
          <div class="field"><label>{{ locale.hk_creditos_iniciales|raw }}</label><input type="number" name="credits" min="0" value="{{ form.credits | default(0) }}"></div>
        </div>
        <div class="row r-2">
          <div class="field"><label>{{ locale.hk_look_opcional|raw }}</label><input type="text" name="figure" class="mono" placeholder="{{ defaultFigure }}" value="{{ form.figure }}"></div>
          <div class="field"><label>{{ locale.hk_sexo|raw }}</label><select name="sex"><option value="M">{{ locale.hk_chico|raw }}</option><option value="F" {% if form.sex == 'F' %}selected{% endif %}>{{ locale.hk_chica|raw }}</option></select></div>
        </div>
        <div class="form-actions"><button class="btn primary" type="submit">{{ locale.hk_crear_usuario|raw }}</button></div>
      </form>
    </div>
  </div>
  <div class="card">
    <div class="card-head"><h2>{{ locale.hk_antes_de_crear|raw }}</h2></div>
    <div class="card-body">
      <p class="muted" style="margin-top:0">{{ locale.hk_la_cuenta_se_crea_con_rango_1_usuario_despues_pu|raw }}</p>
      <p class="muted">{{ locale.hk_para_pruebas_usa_nombres_que_empiecen_por|raw }} <code>{{ locale.hk_prueba|raw }}</code> {{ locale.hk_y_borralos_al_terminar|raw }}</p>
    </div>
  </div>
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
